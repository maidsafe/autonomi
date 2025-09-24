# Launchpad Technical Guide

## Overview

Autonomi Node Launchpad is a terminal user interface that orchestrates node lifecycle operations, visualises live metrics, and keeps user preferences in sync with the local node registry. The application runs entirely in the terminal and relies on a unidirectional action loop to coordinate components.

## Architecture Overview

- **App (`src/app.rs`)** initialises components, maintains the global focus stack, and persists user settings (storage drive, UPnP, port range, rewards wallet, and `nodes_to_start`) to `app_data.json`.
- **Action loop**: user input is translated into `Action` values (via `keybindings.rs`), each component mutates its state, optionally emits follow-up actions, and the app forwards those actions to every component.
- **Asynchronous tasks**: node management work is dispatched to a dedicated `NodeManagement` thread that communicates back through the same action channel, ensuring the TUI thread stays responsive.

## Scenes and Components

### Scene Management

- `Scene` (in `mode.rs`) enumerates the active view: `Status`, `Options`, `Help`, and several popups (Manage Nodes, Change Drive, Rewards Address, Remove Node, Upgrade Nodes, Logs, etc.).
- `InputMode` switches between navigation and text entry, while `FocusManager` enforces which component consumes keyboard events.

### Core Components

- **Status scene**: renders device stats, the node table, footer help, and mirrors `NodeTableActions` to stay up to date with registry changes.
- **Node table**: resides in `components/node_table`. It loads the registry, listens for file watcher updates, locks rows during operations, and fans out node management commands.
- **Options scene**: allows editing of UPnP, port ranges, rewards wallet, and storage drive. Changes emit `Action::StoreUpnpSetting`, `Action::StorePortRange`, or `Action::OptionsActions::UpdateStorageDrive`, which update both Status state and the persisted config.
- **Popups**: Manage Nodes, Change Drive, Rewards Address, Remove Node, Upgrade Nodes, and Logs each take focus until dismissed. They coordinate with the main scenes through the shared action bus.

## Node Lifecycle & Registry Integration

- `NodeRegistryManager` (from `ant-service-management`) is the single source of truth. A watcher on `node_registry.json` fires `NodeTableActions::RegistryUpdated` whenever the file changes.
- `NodeTableState::sync_node_service_data` reconciles node rows, unlocks previously locked entries, and recomputes the running-node count so `nodes_to_start` reflects the actual registry state after manual or background operations.
- `NodeOperations` packages user intentions into `NodeManagementTask`s (add, start, stop, maintain, remove, reset, upgrade).
- `NodeManagement` executes those tasks via `ant-node-manager`, then returns `NodeManagementResponse` messages that unblock the UI and display any errors.

## Configuration, Storage, and Limits

- `nodes_to_start` drives the Maintain Nodes workflow. The Manage Nodes popup enforces a per-node storage requirement of 35 GB and caps the value at `MAX_NODE_COUNT` (50).
- Disk selection and free-space validation happen inside the Change Drive popup, using `sysinfo::Disks` to verify the chosen mountpoint.
- Rewards wallet entry is mandatory before spawning nodes; both Status and Options scenes can trigger the wallet popup when needed (`StatusActions::TriggerRewardsAddress`).
- All configuration values are saved in `app_data.json`, ensuring preferences persist between sessions.

## Logs & Diagnostics

- `Ctrl+T` (or the footer shortcut) opens the Logs popup for the currently selected node.
- `Action::NodeTableActions::TriggerNodeLogs` identifies the target service, and `Action::SetNodeLogsTarget` instructs the popup to stream log lines. Errors surface through `Action::LogsLoadError` so the UI can present troubleshooting feedback.

## Keyboard Highlights

Key shortcuts exposed in the Status scene:

- Manage nodes: `Ctrl+G`
- Start/Stop nodes: `Ctrl+R` / `Ctrl+X`
- Add node: `+`
- Remove node: `-`, `Delete`, or `Ctrl+D`
- Toggle selected node: `Ctrl+S`
- Logs popup: `Ctrl+T`
- Table navigation respects locked rows and supports Arrow, Page Up/Down, Home, and End keys.

## Testing Strategy

### Unit and Component Tests

- `test_utils::make_node_service_data` offers deterministic node snapshots for UI tests without touching real services.
- Rendering checks rely on `ratatui::Terminal<TestBackend>` to capture buffers for assertion.
- Tests cover scenarios where registry sync triggers `Action::StoreRunningNodeCount`, ensuring the action fires exactly once per state change.

### Journey Testing

- `test_utils::journey` defines end-to-end “journey” tests that simulate real user flows (scene navigation, popup interactions, node operations).
- Journeys compose `KeySequence` helpers from `test_utils::keyboard` and assert against rendered buffers, providing cross-component coverage.

## Build & CLI Options

```bash
cargo run --release --bin node-launchpad
```

Useful flags:

- `--network-id <ID>`: connect to a specific network (default: 1).
- `--antnode-path <PATH>`: point to a custom `antnode` binary.
- `--network-contacts-url <URL>` / `--peer <MULTIADDR>`: override bootstrap contacts.

Launchpad requires ~35 GB of free space per node, a valid rewards wallet, and platform-specific permissions to manage services.

All build and test commands respect any pre-set `CARGO_TARGET_DIR`; avoid overwriting the variable so outputs remain in your chosen location.
