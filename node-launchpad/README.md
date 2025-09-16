# Autonomi Node Launchpad

A terminal user interface (TUI) for managing Autonomi network nodes. This tool provides an easy way to set up, monitor, and maintain nodes on the Autonomi decentralized network.

For a deeper technical tour see [agent.md](./agent.md).

## Features

- **Registry-backed node management**: Start, stop, maintain, upgrade, remove, or reset services with the node registry acting as the source of truth.
- **Live resource telemetry**: View per-node rewards, memory, bandwidth, peers, and connection counts in the Status scene.
- **Configurable deployment**: Pick storage mountpoints, port ranges, UPnP behaviour, network ID, and custom `antnode` binaries.
- **Wallet integration & persistence**: Store the rewards wallet and other preferences in `app_data.json` for reuse across sessions.
- **Keyboard-first workflows**: Rich shortcuts for operations, logs, manage-nodes popup, and table navigation.

## Installation

Download the latest version from [docs.autonomi.com/node/downloads](https://docs.autonomi.com/node/downloads) or build from source:

```bash
git clone https://github.com/maidsafe/autonomi
cd autonomi
cargo run --release --bin node-launchpad
```

## Requirements

- 35GB of storage space per node
- Stable internet connection
- Windows, macOS, or Linux operating system
- Administrator/root privileges (required for Windows)

## Usage

Launchpad provides three primary scenes that you can switch between at any time:

| Scene | Purpose | Default Keys |
|-------|---------|--------------|
| **Status** | Real-time node overview, device stats, and quick actions | `s`, `S` |
| **Options** | Configure storage drive, port range, UPnP, rewards wallet | `o`, `O` |
| **Help** | Display keybindings and workflow tips | `h`, `H` |

Popups (Manage Nodes, Change Drive, Rewards Address, Remove Node, Upgrade Nodes, Logs, etc.) capture focus until dismissed, making it easy to complete multi-step flows without stray input.

Essential keybindings inside the Status scene:

- `Ctrl+G`: Open **Manage Nodes** to adjust `nodes_to_start` (enforces 35 GB per node and a 50-node cap).
- `Ctrl+R`: Start all eligible nodes; `Ctrl+X` stops any running nodes.
- `+`: Add a new node (validates wallet, disk space, and node limit).
- `-`, `Delete`, or `Ctrl+D`: Trigger the **Remove Node** confirmation popup.
- `Ctrl+S`: Toggle the selected node between start/stop.
- `Ctrl+T`: Open the **Logs** popup for the highlighted node.

- Logs: `Ctrl+T` opens the real-time log viewer popup. Use `Esc` to close it and return to the Status scene.
- Arrow/Page/Home/End keys navigate the table while skipping locked rows.

Further operator guides live at [docs.autonomi.com/node/guides/how-to-guides](https://docs.autonomi.com/node/guides/how-to-guides).

## Developer Notes

### Connecting to a Custom Network

The launchpad supports connecting to different Autonomi networks. Here is an example on how to spawn nodes using a
pre-built node binary and connect it to a network with a custom network ID.


| Option | Description |
|--------|-------------|
| `--network-id <ID>` | Specify the network ID to connect to. Default is 1 for mainnet |
| `--antnode-path <PATH>` | Path to the pre-built node binary |
| `--network-contacts-url <URL>` | Comma-separated list of URL containing the bootstrap cache. Can be ignored if `--peer` is used |
| `--peer <MULTIADDR>` | Comma-separated list of peer multiaddresses. Can be ignored if `--network-contacts-url` is used |


```bash
./node-launchpad --network-id 2 --antnode-path /path/to/antnode --peer /ip4/1.2.3.4/tcp/12000/p2p/12D3KooWAbCxMV2Zm3Pe4HcAokWDG9w8UMLpDiKpMxwLK3mixpkL
```

## Testing

Launchpad favours deterministic UI tests built on `ratatui::Terminal<TestBackend>` alongside registry fakes.

```bash
# Run the full suite
cargo test --workspace --all-features

# Target a specific scenario (example: registry-driven node count sync)
cargo test -p node-launchpad sync_updates_running_node_count_from_registry

# Lint and format helper (defined in the Justfile)
cargo fmt --all && cargo clippy --all-features --all-targets
```

Testing tips:

- Use `test_utils::MockNodeRegistry` to emulate node lifecycle changes without touching real services.
- Prefer focused `#[tokio::test]` async unit tests inside modules for state machines (e.g. `NodeTableState`).
- Keep integration tests in `tests/` for cross-component rendering and journey coverage.
- When asserting buffers, convert `Terminal` output to strings with helpers from `test_utils::test_helpers`.
