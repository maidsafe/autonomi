# ant-node-manager Agent Notes

## Purpose & Entry Points
- Crate exposes the `antctl` binary (`src/bin/antctl`) for installing and operating `antnode` either as OS services or as ad-hoc local processes.
- Library surface (`src/lib.rs`) re-exports helpers plus the `BatchServiceManager` and is consumed primarily by the CLI.
- Core state is stored via `ant_service_management::NodeRegistryManager`; refresh and reporting happen through `status_report` and `refresh_node_registry`.

## Key Modules
- `cmd/node.rs`: High-level workflows for service-based networks (`add`, `start`, `stop`, `remove`, `reset`, `status`, `upgrade`, `balance`). Wraps lower-level helpers and `BatchServiceManager`.
- `cmd/local.rs`: Local-network lifecycle (`run`, `join`, `kill`, `status`) and management of the auxiliary EVM testnet.
- `add_services`: Builds `ServiceInstallCtx` and copies binaries/dirs. `add_node` handles port validation, filesystem prep, registry updates, and optional env var persistence.
- `batch_service_manager`: Batch orchestration for `NodeService` operations with progress tracking, restart checks, and upgrade/remove flows.
- `local.rs`: Launches unmanaged local `antnode` processes, records them in the registry, and validates connectivity; also tears down nodes and cleans directories.
- `helpers.rs`: Shared tooling for downloads (`download_and_extract_release`), version probing, WinSW install, temp dirs, and port-range utilities.
- `config.rs`: Cross-platform directory resolution, privilege detection, and helper to create owned directories for system-wide installs.
- `src/bin/antctl/commands.rs`: Clap definitions. Look here for CLI arguments, defaults, and `EvmNetworkCommand` parsing.

## Service Management Workflow
1. **Add (`cmd::node::add`)**
   - Resolve binary path (download via S3/URL or reuse local path) and version.
   - Build `AddNodeServiceOptions`, validating optional port ranges (`PortRange`).
   - `add_node` copies the binary per service, creates data/log directories (with owner adjustments on Unix), and invokes `service_control.install`.
   - Newly added nodes are appended to the registry with status `Added`; env vars snapshot stored in the registry if provided.
2. **Start (`BatchServiceManager::start_all`)**
   - Optionally skip startup validation (`--no-startup-check`).
   - Uses `service_control.start`, waits `fixed_interval` between nodes, then polls `ServiceStartupStatus` until success/timeout. On success, `NodeService::on_start` populates PID, peers, metrics.
3. **Stop (`BatchServiceManager::stop_all`)**
   - Guards against already stopped or never-started services. Updates registry via `on_stop`.
4. **Remove (`BatchServiceManager::remove_all`)**
   - Refuses to remove running services (unless they already died); optionally removes data/log directories.
5. **Upgrade (`BatchServiceManager::upgrade_all`)**
   - Acquires upgrade binary (custom path, version, or latest). Supports `--force` and `--do-not-start`.
   - Each service is stopped, binary replaced, uninstalled, and re-installed with the previous install context (including env vars/max logs etc.).
   - Batch start may follow; summary returned per-service.
6. **Reset (`cmd::node::reset`)**
   - Prompts (unless `--force`), then stops and removes everything, finally deletes the registry file if present.

## Local Network Lifecycle
- `cmd::local::{run, join}` delegate to `local::run_network`.
  - Validates optional port ranges when joining an external local network.
  - For fresh networks, spawns first node with `--first` and records metrics/listen addrs; subsequent nodes reuse metrics/rpc port allocation helpers.
  - Rewards address and EVM network are required; local mode always skips reachability checks and uses `--local`.
  - `validate_network` probes metrics to report peer counts.
- `local::kill_network` enumerates tracked nodes, terminates processes by PID, optionally deletes per-node directories, and kills lingering `evm-testnet/anvil` processes.
- EVM testnet helpers (`cmd/local.rs`) ensure a background chain is available: build or locate the binary, spawn it, and poll for readiness.

## Node Registry & File Layout
- Registry path from `config::get_node_registry_path()`:
  - Unix root installs: `/var/antctl/node_registry.json` (world-writable for status refreshes).
  - Unix user-mode / macOS: `~/.local/share/autonomi/node/node_registry.json` (under data dir).
  - Windows: `C:\ProgramData\antctl\node_registry.json`.
- Data/log directories depend on privilege:
  - System services: `/var/antctl/services/<name>` and `/var/log/antnode/<name>`.
  - User services: `~/.local/share/autonomi/node/<name>` with nested `logs`.
  - Windows defaults under `C:\ProgramData\antctl`.
- Registry entries (`NodeServiceData`) store metrics port, listen addrs, PID, peer ID, rewards address, and initial peer config; `environment_variables` field on the manager holds last provided env pairs.

## Notable Types & Utilities
- `VerbosityLevel` (1–3) toggles console output, progress bars, and logging.
- `DEFAULT_NODE_STARTUP_INTERVAL_MS` (10s) is the default inter-node start delay for batches.
- `PortRange` accepts single port (`"1234"`) or inclusive ranges (`"12000-12010"`), validates counts, and checks runtime conflicts against existing services/ports recorded in the registry.
- `helpers::download_and_extract_release` handles retries, cached archives, progress reporting, and derives binary version via `--version`.
- `helpers::configure_winsw` installs WinSW as needed and exports `WINSW_PATH` (no-op on non-Windows).

## CLI Highlights (`antctl`)
- Global flags: `--debug/--trace`, `--verbose`, `--log-output-dest`, `--crate-version`, `--version`.
- Service commands: `add`, `start`, `stop`, `status`, `upgrade`, `remove`, `reset`, `balance`.
- Local network namespace: `local run`, `local join`, `local kill`, `local status` with optional `--build`, port overrides, and EVM selection (`evm-{arbitrum-one,sepolia-test,local,custom}`).
- Most commands accept `--service-name` and/or `--peer-id` filters; defaults operate on all tracked services.

## Error Handling & Feedback
- Errors use `crate::error::Error`; notable variants: `ServiceBatchOperationFailed`, `ServiceNotRunning`, `DownloadFailure`, `PortInUse`, `ServiceProgressTimeout`, and wrappers around `ant_service_management` errors.
- Batch operations accumulate errors per service in `BatchResult`; `summarise_batch_result` prints failures when verbosity permits and bubbles an error.
- Status output supports JSON (`--json`) for machine consumption; otherwise pretty tables or banners controlled by verbosity.

## Testing & Tooling
- Unit tests focus on builders/config (`add_services::config`, `local` module with mocks, `batch_service_manager`).
- Integration test (`tests/e2e.rs`) expects root privileges, prebuilt `antnode`, and a running network (suitable for CI/vagrant VM). Helper utilities live in `tests/utils.rs`.
- Vagrantfile provided for integration environment; README documents VM workflow (`just node-man-integration-tests`).

## External Dependencies to Note
- `ant_service_management`: core abstraction for service control, filesystem helpers, metrics retrieval, and registry persistence.
- `ant_releases`: interacts with release storage (S3 or URLs) for fetching binaries, plus `ReleaseType` enums.
- `ant_bootstrap`: initial peer configurations; used when adding services and bootstrapping local networks.
- `ant_evm`: reward address parsing, EVM network definitions, and custom network wiring; local mode expects an `evm-testnet` process.
- `service-manager`: cross-platform OS service creation/management.
- `indicatif`: user-facing progress bars; templates live in helpers and batch manager.

## Operational Tips
- Ensure `configure_winsw` runs before any Windows service install (main does this automatically).
- Refreshing the registry (`status_report` or `refresh_node_registry`) should precede operations that rely on runtime state.
- When running upgrades with custom binaries, the command implicitly forces the upgrade even if the target version is lower.
- Local network nodes skip reachability checks and rely on metrics connectivity to determine liveness.
- For user-mode services, directory ownership is handled automatically; for system services, the CLI will create or reuse a `ant` (default) service user unless `--user` overrides it.

