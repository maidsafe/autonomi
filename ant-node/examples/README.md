# Node spawner examples

This directory contains examples for programmatically spawning Autonomi nodes.

## Examples

| Example | Description |
|---------|-------------|
| `spawn_local_network.rs` | Spawn a local network with EVM testnet, connect a client, get a cost quote |
| `spawn_nodes_on_mainnet.rs` | Spawn nodes on the live Autonomi mainnet |

## Running examples

```bash
# Local network (requires Foundry/Anvil)
cargo run --example spawn_local_network

# Mainnet nodes (set your wallet/rewards address to receive rewards)
REWARDS_ADDRESS=0xYourWalletAddress cargo run --example spawn_nodes_on_mainnet
```

---

## NodeSpawner API

`NodeSpawner` is used to spawn a single Autonomi node.

### Basic usage

```rust
use ant_node::spawn::node_spawner::NodeSpawner;

let node = NodeSpawner::new()
    .with_evm_network(EvmNetwork::ArbitrumOne)
    .with_rewards_address(my_wallet_address)
    .with_relay_client(true)
    .spawn()
    .await?;
```

### Builder methods

| Method | Default | Description |
|--------|---------|-------------|
| `.with_socket_addr(addr)` | `0.0.0.0:0` | IP and port to listen on. `0.0.0.0` = all interfaces, `:0` = OS picks port |
| `.with_evm_network(network)` | `ArbitrumOne` | Blockchain for payments. See [EvmNetwork](#evmnetwork-options) |
| `.with_rewards_address(addr)` | `0x0...0` | **Your wallet address for rewards.** Default burns rewards! |
| `.with_bootstrap_config(cfg)` | `None` | How to find peers. See [BootstrapConfig](#bootstrapconfig-options) |
| `.with_no_upnp(bool)` | `false` | `true` = disable UPnP port forwarding. See [NAT & Connectivity](#nat--connectivity) |
| `.with_relay_client(bool)` | `false` | `true` = use relay servers for NAT traversal. See [NAT & Connectivity](#nat--connectivity) |
| `.with_root_dir(path)` | System default | Directory to store node data and keys |

---

## NetworkSpawner API

`NetworkSpawner` spawns multiple nodes that form a network together.

### Basic usage

```rust
use ant_node::spawn::network_spawner::NetworkSpawner;

let network = NetworkSpawner::new()
    .with_evm_network(evm_network)
    .with_bootstrap_config(BootstrapConfig::new(true))
    .with_size(25)
    .spawn()
    .await?;

// Get bootstrap peer for clients
let bootstrap_peer = network.bootstrap_peer().await;

// Access individual nodes
for node in network.running_nodes() {
    println!("Node: {}", node.peer_id());
}

// Cleanup
network.shutdown();
```

### Builder methods

| Method | Default | Description |
|--------|---------|-------------|
| `.with_evm_network(network)` | `ArbitrumOne` | Blockchain for payments |
| `.with_rewards_address(addr)` | `0x0...0` | **Your wallet address.** Default burns rewards! |
| `.with_bootstrap_config(cfg)` | `None` | How nodes find each other. See [BootstrapConfig](#bootstrapconfig-options) |
| `.with_no_upnp(bool)` | `false` | Disable UPnP for all nodes |
| `.with_root_dir(path)` | System default | Base directory for all node data |
| `.with_size(n)` | `5` | Number of nodes to spawn |

### RunningNetwork methods

| Method | Description |
|--------|-------------|
| `.bootstrap_peer()` | Get a multiaddr to connect clients to this network |
| `.running_nodes()` | Get reference to all `RunningNode` instances |
| `.shutdown()` | Stop all nodes gracefully |

---

## EvmNetwork options

The blockchain network for payment processing.

```rust
use ant_evm::EvmNetwork;

// Production (real tokens)
EvmNetwork::ArbitrumOne

// Testnet (test tokens)
EvmNetwork::ArbitrumSepoliaTest

// Local development (use with evmlib::testnet::Testnet)
// Pass the result of testnet.to_network()
```

| Network | Use Case | Tokens |
|---------|----------|--------|
| `ArbitrumOne` | Production mainnet | Real ANT tokens |
| `ArbitrumSepoliaTest` | Public testnet | Test tokens |
| Custom (from `Testnet`) | Local development | Fake tokens |

---

## BootstrapConfig options

Controls how nodes discover peers on the network.

### Basic usage

```rust
use ant_node::BootstrapConfig;

// Local network (isolated, no external peers)
let config = BootstrapConfig::new(true);

// Live network (connects to mainnet/testnet peers)
let config = BootstrapConfig::new(false);
```

### Constructor

```rust
BootstrapConfig::new(local: bool)
```

- `local = true`: Isolated local network, won't fetch external peers
- `local = false`: Connect to live network peers

### Builder methods

| Method | Default | Description |
|--------|---------|-------------|
| `.with_first(bool)` | `false` | `true` = this is the first/genesis node (doesn't need bootstrap peers) |
| `.with_initial_peers(vec)` | `[]` | Specific peer addresses to connect to |
| `.with_local(bool)` | from constructor | Override local network flag |
| `.with_disable_cache_reading(bool)` | `false` | Don't read cached peers from disk |
| `.with_disable_cache_writing(bool)` | `false` | Don't save peers to disk cache |
| `.with_disable_env_peers(bool)` | `false` | Ignore `ANT_PEERS` environment variable |
| `.with_cache_dir(path)` | System default | Custom path for peer cache |
| `.with_network_contacts_url(urls)` | Default URLs | Custom URLs to fetch bootstrap peers |
| `.with_max_concurrent_dials(n)` | `10` | Max parallel connection attempts |
| `.with_max_contacted_peers_before_termination(n)` | `5` | Stop bootstrapping after N successful peers |
| `.with_max_cached_peers(n)` | `1500` | Max peers to store in cache |
| `.with_max_addrs_per_cached_peer(n)` | `3` | Max addresses per peer in cache |

### Common patterns

```rust
// Local network (testing)
BootstrapConfig::new(true)
    .with_disable_cache_reading(true)
    .with_disable_env_peers(true)

// Live network (production)
BootstrapConfig::new(false)
    .with_first(false)

// First node in local network
BootstrapConfig::new(true)
    .with_first(true)
```

---

## NAT & Connectivity

Most home internet connections are behind NAT (Network Address Translation), which blocks incoming connections.

### The Problem

```
Without NAT traversal:
[Mainnet Peers] --X--> [Your Router NAT] --X--> [Your Node]
                       (blocked!)
```

### Solutions

| Flag | What it does | When to use |
|------|--------------|-------------|
| `no_upnp = false` (default) | Try UPnP to open router ports automatically | Router supports UPnP |
| `relay_client = true` | Connect via relay servers | Behind NAT, UPnP doesn't work |

### How they work together

```
                                    ┌─────────────────┐
                                    │  Your Router    │
                                    │  (NAT)          │
                                    └────────┬────────┘
                                             │
┌─────────────────┐                          │
│ UPnP Attempt    │◄─────────────────────────┤
│ (no_upnp=false) │  "Open port 12345 please"│
└─────────────────┘                          │
        │                                    │
        ▼                                    │
   Success? ────Yes───► Direct connections work!
        │
        No
        │
        ▼
┌─────────────────┐     ┌─────────────────┐
│ Relay Mode      │────►│ Relay Server    │◄──── Mainnet Peers
│ (relay=true)    │     │ (on internet)   │
└─────────────────┘     └─────────────────┘
```

### Recommended settings

```rust
// Home connection (most users)
.with_relay_client(true)  // Safe fallback, only used if needed

// Data center / public IP
// Defaults are fine (no_upnp=false, relay=false)
```

### Important notes

- **Relay is a fallback**: Even with `relay_client = true`, direct connections are tried first
- **Safe to always enable**: Relay only activates when direct connections fail
- **No conflicts**: UPnP and relay work together, not against each other

---

## RewardsAddress

Your Ethereum wallet address for receiving node rewards.

```rust
use ant_evm::RewardsAddress;
use std::str::FromStr;

// Your real wallet address
let addr = RewardsAddress::from_str("YOUR REWARD/WALLET ADDRESS")?;

// Default = zero address (0x0000...0000)
// WARNING: Rewards sent here are BURNED (lost forever)!
let zero = RewardsAddress::default();
```

**Always set your rewards address for production!** The default zero address means all rewards are permanently lost.

---

## RunningNode methods

After spawning, you get a `RunningNode` with these methods:

| Method | Description |
|--------|-------------|
| `.peer_id()` | Get the node's libp2p PeerId |
| `.get_listen_addrs()` | Get listening addresses (without peer ID) |
| `.get_listen_addrs_with_peer_id()` | Get full multiaddrs (with peer ID) |
| `.get_swarm_local_state()` | Get network stats (peers, connections) |
| `.shutdown()` | Stop the node gracefully |

### Example: Check node status

```rust
let state = node.get_swarm_local_state().await?;
println!("Peers in routing table: {}", state.peers_in_routing_table);
println!("Connected peers: {}", state.connected_peers.len());
```

---

## Complete examples

### Local network for testing

```rust
use ant_evm::RewardsAddress;
use ant_node::BootstrapConfig;
use ant_node::spawn::network_spawner::NetworkSpawner;
use evmlib::testnet::Testnet;

// Start local EVM
let testnet = Testnet::new().await;
let evm_network = testnet.to_network();

// Spawn 25 nodes
let network = NetworkSpawner::new()
    .with_evm_network(evm_network)
    .with_rewards_address(RewardsAddress::default())  // OK for testing
    .with_bootstrap_config(BootstrapConfig::new(true))
    .with_size(25)
    .spawn()
    .await?;

// Connect client
let bootstrap_peer = network.bootstrap_peer().await;
let client = Client::init_with_peers(vec![bootstrap_peer]).await?;

// Use the network...

network.shutdown();
```

### Mainnet nodes

Run with your wallet address:
```bash
REWARDS_ADDRESS=0xYourWalletAddress cargo run --example spawn_nodes_on_mainnet
```

```rust
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_node::BootstrapConfig;
use ant_node::spawn::node_spawner::NodeSpawner;
use std::env;
use std::str::FromStr;

// Set mainnet network ID
ant_protocol::version::set_network_id(1);

// Set env var REWARDS_ADDRESS or replace "YOUR_WALLET/REWARDS_ADDRESS_HERE" with your address
let rewards_address_str = env::var("REWARDS_ADDRESS")
    .unwrap_or_else(|_| "YOUR_WALLET/REWARDS_ADDRESS_HERE".to_string());
let rewards = RewardsAddress::from_str(&rewards_address_str)
    .expect("Invalid address!");

// Spawn node with relay for NAT traversal
let node = NodeSpawner::new()
    .with_evm_network(EvmNetwork::ArbitrumOne)
    .with_rewards_address(rewards)
    .with_bootstrap_config(BootstrapConfig::new(false).with_first(false))
    .with_relay_client(true)
    .spawn()
    .await?;

println!("Node {} connected to mainnet!", node.peer_id());
```
