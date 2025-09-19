# Node Table Architecture

The node table is the central status view for Launchpad. It is implemented as a collection of small modules that each focus on a single responsibility. At a high level the flow is:

```
Registry feed ─────┐
                   ├─> NodeStateController ─┬─> NodeViewModel list ─┬─> Widgets (table + spinners)
User intent ───────┘                         │                      │
                                             │                      └─> Status / Screen fixtures
Transitions ─────────────────────────────────┘
```

## Module Overview

- `lifecycle.rs`
  - Defines the immutable data shared by the rest of the component:
    - `RegistryNode`: lightweight representation of a node record coming from the registry.
    - `DesiredNodeState`, `CommandKind`, `TransitionEntry`: intent + transition helpers.
    - `LifecycleState` and `NodeViewModel`: derived state used to render the UI.
  - Provides helper functions (`derive_lifecycle_state`, `build_view_models`) and the `NodeMetrics` structure that bundles telemetry values.

- `state.rs`
  - `NodeStateController`: owns per-node state (`NodeState` holds registry data, desired state, transitions, metrics, reachability, bandwidth totals), the desired running count, and the `StatefulTable<NodeViewModel>` used by the UI. It refreshes the view model whenever registry data, user intent, or transitions change.
  - `NodeTableState`: Launchpad’s integration layer. It invokes the controller, orchestrates reconciliation, maintains configuration values, and bridges to actions (`Action::NodeTableActions`). Selection/navigation logic now delegates to the controller’s table state.

- `operations.rs`
  - Wraps the async `NodeManagement` commands. It receives structured configs (maintain, add, start, stop, remove, upgrade) and proxies them to the management task runner. The component calls these helpers and updates the controller’s transition state around them.

- `widget.rs`
  - Renders the table using the controller’s `NodeViewModel` list. The widget chooses layouts dynamically, shows progress bars while reachability checks are running, and uses the lifecycle state to select spinner styles.

- `mod.rs`
  - `NodeTableComponent`: Launchpad’s Component implementation. It wires the action handler, spawns the registry watcher, forwards user intent into `NodeStateController`, and answers UI events.

- `state.rs`
  - Exposes `NodeSelectionInfo` so external components (status footer, popups) can react to the currently selected row without depending on legacy enums. All other structural node data now lives in `NodeViewModel`.

## Data Flow

1. **Registry updates**: the watcher sends `NodeTableActions::RegistryUpdated`. `NodeTableState::sync_node_service_data` stores the latest `NodeServiceData` list, merges them into the controller’s `NodeState` map, reconciles transitions, refreshes the view model, and emits state/selection updates.

2. **User intent**: commands such as `StartNodes` or `StopNodes` collect eligible `NodeViewModel`s, set per-node targets/transition markers on the controller, and dispatch the corresponding operation via `NodeOperations`. When the registry later reflects the change, reconciliation clears the transition and unlocks the rows.

3. **Metrics/reachability**: aggregated stats and reachability reports feed into `NodeStateController::update_metrics` / `update_reachability`, which refresh the view model so the UI reflects live telemetry.

## Selection & Navigation

Selection is stored in the controller’s `StatefulTable<NodeViewModel>`. Navigation helpers (`navigate_*_unlocked`) always consult the view model and respect locked states that indicate in-flight transitions. Each movement triggers `send_selection_update` so other components (e.g. popups) stay in sync.

## Why This Exists

The previous implementation intertwined registry snapshots, user intent, and UI bookkeeping inside `NodeTableState`, making intent fragile (`nodes_to_start` was clobbered during sync) and spinner logic error-prone. The new architecture separates concerns:

- **Registry data is merged** into long-lived `NodeState` entries.
- **Intent is explicit per node** and only mutated by user actions.
- **Transitions** model in-flight commands.
- **Lifecycle** collapses the three into a deterministic view model consumed by the UI.

This separation allowed us to delete large amounts of ad-hoc locking code, remove `NodeItem` from the runtime path, and make the screen render purely from derived state.

## Testing

- Lifecycle derivation is unit-tested (`lifecycle.rs` tests).
- The existing integration tests (screen journeys, operations, popups) now cover the lifecycle-driven table because they exercise the same public APIs.
- Rendering snapshots have been updated to assert the bandwidth columns and startup progress bar now that those come directly from `NodeViewModel` metrics.

## Extending the Table

- To add a new column: extend `NodeMetrics` (or add to `NodeViewModel`), update `build_view_models`, and adjust `widget.rs` formatting helpers.
- To support a new command: add a `CommandKind` variant, update transition reconciliation, and wire a new handler in `NodeTableComponent` that sets desired state and invokes `NodeOperations`.
- To expose new intent (e.g. pinning a node): add methods on `NodeStateController` to adjust `NodeState::desired`, then surface the intent through actions.

With the lifecycle controller in place, UI changes no longer need to manipulate registry structs directly—only the controller’s APIs.
