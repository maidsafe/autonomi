# Reachability Check Feature

## Overview

The **reachability check** is a network connectivity assessment feature that determines whether a node in the Autonomi Network can be reached directly from the internet or needs to use relay connections. This feature is critical for ensuring optimal network performance and connectivity in diverse network environments.

### What is Reachability?

In peer-to-peer networks, nodes need to be able to connect to each other to exchange data and maintain the network. However, many nodes run behind firewalls, routers with Network Address Translation (NAT), or in other network configurations that prevent direct connections from the internet. The reachability check automatically detects these situations and configures the node accordingly.

### Why is This Important?

- **Automatic Configuration**: Eliminates the need for manual network configuration
- **Optimal Performance**: Ensures nodes use the most efficient connection method available
- **Network Stability**: Prevents nodes from joining the network if they cannot maintain stable connections
- **User Experience**: Provides clear feedback about connectivity status and configuration

## How It Works

The reachability check follows a systematic approach to determine connectivity:

1. **Network Setup**: Creates a specialized network swarm with minimal behaviors (UPnP and Identify)
2. **Peer Discovery**: Attempts to connect to multiple bootstrap peers from the network
3. **Connection Testing**: Establishes connections and waits for peers to identify the node
4. **Observation Collection**: Gathers information about how other peers see this node's network address
5. **Analysis**: Analyzes the collected data to determine the node's reachability status
6. **Decision**: Returns a reachability status that determines how the node should configure its networking

## Reachability Status Types

The check returns one of three possible statuses:

### 1. **Reachable** 
- The node can be reached directly from the internet
- No relay is needed
- Returns the external address where the node is reachable
- Includes UPnP support information

### 2. **Relay**
- The node is behind NAT/firewall and cannot be reached directly
- Must use relay connections for other nodes to reach it
- Can still make outbound connections normally
- Includes UPnP support information

### 3. **NotRoutable**
- The node cannot be reached at all, even with relay assistance
- Usually indicates severe networking issues
- The node should not join the network in this state

## Technical Implementation

The reachability check module is designed to determine a node's network connectivity status by testing its ability to establish and receive connections from other peers in the network. The module implements a sophisticated state machine that manages peer connections, collects network observations, and analyzes the data to make reachability determinations.

### 1. ReachabilityCheckSwarmDriver (`mod.rs`)

The main orchestrator that manages the entire reachability detection process.

```rust
pub(crate) struct ReachabilityCheckSwarmDriver {
    pub(crate) swarm: Swarm<ReachabilityCheckBehaviour>,
    pub(crate) upnp_supported: bool,
    pub(crate) dial_manager: DialManager,
    pub(crate) listeners: HashMap<ListenerId, HashSet<IpAddr>>,
}
```

**Responsibilities:**
- Manages the libp2p swarm with specialized behaviors
- Coordinates the detection workflow
- Processes network events and maintains state
- Makes final reachability determinations

### 2. DialManager (`dialer.rs`)

A sophisticated connection management system that handles peer discovery and connection tracking.

```rust
pub(crate) struct DialManager {
    pub(crate) current_workflow_attempt: usize,
    pub(crate) dialer: Dialer,
    pub(crate) all_dial_attempts: HashMap<PeerId, DialResult>,
    pub(crate) initial_contacts_manager: InitialContactsManager,
}
```

**Key Features:**
- **Workflow Retry Management**: Tracks retry attempts across entire detection cycles
- **Dial State Tracking**: Maintains connection states for all dial attempts
- **Bootstrap Contact Management**: Efficiently manages and selects bootstrap peers
- **Result Aggregation**: Collects and analyzes dial attempt outcomes

### 3. Dialer (`dialer.rs`)

The core connection tracking component that maintains real-time state of all network operations.

```rust
pub(crate) struct Dialer {
    ongoing_dial_attempts: HashMap<PeerId, DialState>,
    pub(super) identify_observed_external_addr: HashMap<PeerId, Vec<(SocketAddr, ConnectionId)>>,
    pub(super) incoming_connection_ids: HashSet<ConnectionId>,
    pub(super) incoming_connection_local_adapter_map: HashMap<ConnectionId, SocketAddr>,
}
```

**Data Structures:**
- **ongoing_dial_attempts**: Real-time tracking of active connection attempts
- **identify_observed_external_addr**: Collection of observed addresses from peers
- **incoming_connection_ids**: Set of connections initiated by remote peers
- **incoming_connection_local_adapter_map**: Maps connection IDs to local network adapters

## State Machine Design

### Dial State Transitions

The module implements a three-state machine for tracking individual peer connections:

```rust
pub(super) enum DialState {
    Initiated { at: Instant },
    Connected { at: Instant },
    DialBackReceived { at: Instant },
}
```

**State Transitions:**
1. **Initiated**: Initial dial attempt sent to peer
2. **Connected**: Successful connection established, waiting for identify response
3. **DialBackReceived**: Peer has sent identify information back

**Transition Logic:**
- `Initiated → Connected`: When connection is successfully established
- `Connected → DialBackReceived`: When identify response is received after `DIAL_BACK_DELAY`
- States can timeout and be cleaned up if they exceed their respective timeouts

### Dial Result Tracking

Final outcomes of dial attempts are tracked separately to ensure comprehensive analysis:

```rust
pub(crate) enum DialResult {
    TimedOutOnInitiated,
    TimedOutAfterConnecting,
    ErrorDuringDial,
    SuccessfulDialBack,
}
```

## Network Behavior Configuration

### ReachabilityCheckBehaviour

The module uses a minimal set of libp2p behaviors to reduce complexity and focus on connectivity testing:

```rust
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "ReachabilityCheckEvent")]
pub(crate) struct ReachabilityCheckBehaviour {
    pub(super) upnp: libp2p::upnp::tokio::Behaviour,
    pub(super) identify: libp2p::identify::Behaviour,
}
```

**Behavior Selection Rationale:**
- **UPnP**: Detects and configures NAT traversal capabilities
- **Identify**: Collects observed address information from peers
- **Minimal Set**: Reduces potential interference and simplifies event handling

## Algorithm Details

### 1. Bootstrap Peer Selection

The `InitialContactsManager` implements intelligent peer selection:

```rust
pub(crate) fn get_next_contact(&mut self) -> Option<Multiaddr> {
    // Filters out circuit addresses and ensures PeerID presence
    // Uses random selection to avoid bias
    // Tracks attempted indices to prevent duplicates
}
```

**Selection Criteria:**
- Excludes P2P circuit addresses (relay addresses)
- Requires valid PeerID in the multiaddress
- Uses random selection to distribute load
- Maintains attempt tracking to avoid retries

### 2. Connection Concurrency Management

The system limits concurrent dial attempts to prevent resource exhaustion:

```rust
pub(crate) fn can_we_perform_new_dial(&self) -> bool {
    self.dialer.ongoing_dial_attempts.len() < MAX_DIAL_ATTEMPTS
}
```

**Concurrency Strategy:**
- Maximum 5 concurrent dial attempts (`MAX_DIAL_ATTEMPTS`)
- Attempts are cleaned up on timeout or completion
- New attempts are triggered as slots become available

### 3. Reachability Analysis Algorithm

The core reachability determination logic in `determine_reachability_via_external_addr()`:

```rust
fn determine_reachability_via_external_addr(&self) -> Result<ExternalAddrResult, ReachabilityCheckError>
```

**Analysis Steps:**

#### Step 1: Data Validation
- Ensures minimum 3 observed addresses for reliable analysis
- Validates that at least some connections were successful
- Determines if retry is needed based on data quality

#### Step 2: Port Consistency Check
```rust
if ports.len() != 1 {
    error!("Multiple ports observed, we are unreachable. Terminating the node.");
    result.terminate = true;
    return Ok(result);
}
```

**Rationale**: Multiple observed ports indicates unreachable nodes.


#### Step 3: IP Address Analysis
The algorithm categorizes observed IP addresses:

- **Single IP Address**: 
  - Private IP → `Reachable` (local network scenario)
  - Public IP → `Reachable` (direct internet connectivity)
  - Invalid IP → `NotRoutable`

- **Multiple IP Addresses**:
  - Mixed private/public → Prefer private (local testnet scenario)
  - Multiple private → Prefer localhost, then others
  - Multiple public → Return all as reachable

#### Step 4: Local Adapter Mapping
```rust
let mut external_to_local_addr_map: HashMap<SocketAddr, HashSet<SocketAddr>> = HashMap::new();
```

Maps external observed addresses to local network adapters:
- Prioritize local adapter address that is the same as external address
- Prioritize local adapter address that is the private network range
- Falls back to an non-unspecified local adapter address if found

## Timeout and Retry Strategy

### Timeout Configuration

```rust
const TIMEOUT_ON_INITIATED_STATE: Duration = Duration::from_secs(30);
const TIMEOUT_ON_CONNECTED_STATE: Duration = Duration::from_secs(20 + DIAL_BACK_DELAY.as_secs());
const DIAL_BACK_DELAY: Duration = Duration::from_secs(180);
```

**Timeout Rationale:**
- **INITIATED**: 30 seconds allows for network latency and connection establishment
- **CONNECTED**: 200 seconds total (20 + 180) accounts for dial-back delay plus processing time
- **DIAL_BACK_DELAY**: 180 seconds gives peers time to process and respond with identify

### Retry Mechanism

The module implements a three-tier retry strategy:

1. **Individual Dial Retries**: Automatic cleanup and retry of failed connections
2. **Workflow Retries**: Complete restart of the detection process (max 3 attempts)
3. **Graceful Degradation**: Falls back to relay mode if direct connectivity fails

## Event Processing Pipeline

### Main Event Loop

```rust
pub(crate) async fn detect(mut self) -> Result<ReachabilityStatus, NetworkError> {
    loop {
        tokio::select! {
            swarm_event = self.swarm.select_next_some() => {
                // Handle network events
            }
            _ = dial_check_interval.tick() => {
                // Periodic dial management and cleanup
            }
        }
    }
}
```

### Event Handler Dispatch

The `handle_swarm_events()` method processes different event types:

1. **NewListenAddr**: Updates local address tracking
2. **IncomingConnection**: Maps connection IDs to local adapters
3. **ConnectionEstablished**: Transitions dial states
4. **OutgoingConnectionError**: Handles connection failures
5. **Identify Events**: Processes observed address information
6. **UPnP Events**: Determines NAT traversal capabilities

## Data Collection and Analysis

### Observed Address Collection

```rust
fn insert_observed_address(&mut self, src_peer: PeerId, address: Multiaddr, connection_id: ConnectionId)
```

**Collection Strategy:**
- Associates each observed address with the reporting peer
- Tracks connection IDs for local adapter mapping
- Validates address format and reachability

### Completion Detection

```rust
pub(crate) fn has_dialing_completed(&self) -> bool {
    // Checks if all dial attempts have reached final states
    // Accounts for ongoing timeouts and expected responses
}
```

**Completion Criteria:**
- No active dial attempts in `Initiated` state
- All `Connected` states have either received responses or timed out
- Sufficient data collected for analysis

## Conclusion

The reachability check feature provides essential network connectivity assessment for the Autonomi Network. It automatically determines the optimal networking configuration for each node, improving both performance and reliability. By understanding how it works and how to integrate it properly, developers can ensure their nodes join the network with optimal connectivity settings.

For additional support or questions about the reachability check feature, refer to the main project documentation or reach out to the development team.