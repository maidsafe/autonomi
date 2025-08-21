# The Autonomi Network

[Autonomi.com](https://autonomi.com/) | [Documentation](https://docs.autonomi.com/)

Critical infrastructure of the next web. Assembled from everyday devices and owned by us all.

Autonomi is a fully autonomous data and communications network that provides:
- **Lifetime storage** with one-time payment
- **Private by design** with multilayered encryption
- **Blockchainless data** architecture
- **Decentralized infrastructure** built from everyday devices
- **Content-addressable storage** using Kademlia and libp2p

## Table of Contents

- [For Users](#for-users)
- [For Developers](#for-developers)
- [For the Technical](#for-the-technical)
- [Using a Local Network](#using-a-local-network)
- [Metrics Dashboard](#metrics-dashboard)

### For Users

- [CLI](https://github.com/maidsafe/autonomi/blob/main/ant-cli/README.md) The client command line
  interface that enables users to interact with the network from their terminal.
- [Node](https://github.com/maidsafe/autonomi/blob/main/ant-node/README.md) The backbone of the
  Autonomi network. Nodes can run on commodity hardware and provide storage space and validate
  transactions on the network.
- Web App: Coming Soon!

#### Building the Node from Source

If you wish to build a version of `antnode` from source, some special consideration must be given
if you want it to connect to the current beta network.

You should build from the `stable` branch, as follows:

```
git checkout stable
cargo build --release --bin antnode
```

#### Running the Node

To run a node and receive rewards, you need to specify your Ethereum address as a parameter. Rewards are paid to the specified address.

```
cargo run --release --bin antnode -- --rewards-address <YOUR_ETHEREUM_ADDRESS_TO_RECEIVE_REWARDS>
```

More options about EVM Network below.

### For Developers
#### Main Crates

- [Autonomi API](https://github.com/maidsafe/autonomi/blob/main/autonomi/README.md) The client APIs
  allowing use of the Autonomi network to users and developers.
- [Autonomi CLI](https://github.com/maidsafe/autonomi/blob/main/ant-cli/README.md) The client command line
  interface that enables users to interact with the network from their terminal.
- [Node](https://github.com/maidsafe/autonomi/blob/main/ant-node/README.md) The backbone of the
  Autonomi network. Nodes can be run on commodity hardware and connect to the network.
- [Node Manager](https://github.com/maidsafe/autonomi/blob/main/ant-node-manager/README.md) Use
  to create a local network for development and testing.

#### Transport Protocols and Architectures

The Autonomi network uses `quic` as the default transport protocol.


### For the Technical

- [Logging](https://github.com/maidsafe/autonomi/blob/main/ant-logging/README.md) The
  generalised logging crate used by the autonomi network (backed by the tracing crate).
- [Metrics](https://github.com/maidsafe/autonomi/blob/main/ant-metrics/README.md) The metrics crate
  used by the autonomi network.
- [Networking](https://github.com/maidsafe/autonomi/blob/main/ant-networking/README.md) The
  networking layer, built atop libp2p which allows nodes and clients to communicate.
- [Protocol](https://github.com/maidsafe/autonomi/blob/main/ant-protocol/README.md) The protocol
  used by the autonomi network.
- [Bootstrap](https://github.com/maidsafe/autonomi/blob/main/ant-bootstrap/README.md)
  The network bootstrap cache or: how the network layer discovers bootstrap peers.
- [Build Info](https://github.com/maidsafe/autonomi/blob/main/ant-build-info/README.md) Small
  helper used to get the build/commit versioning info for debug purposes.

### Using a Local Network

We can explore the network's features by using multiple node processes to form a local network. We
also need to run a local EVM network for our nodes and client to connect to.

Follow these steps to create a local network:

##### 1. Prerequisites

The latest version of [Rust](https://www.rust-lang.org/learn/get-started) should be installed. If you already have an installation, use `rustup update` to get the latest version.

Run all the commands from the root of this repository.

If you haven't already, install Foundry. We need to have access to Anvil, which is packaged with Foundry, to run an EVM node: https://book.getfoundry.sh/getting-started/installation

To collect rewards for you nodes, you will need an EVM address, you can create one using [metamask](https://metamask.io/).

##### 2. Run a local EVM node

```sh
cargo run --bin evm-testnet
```

This creates a CSV file with the EVM network params in your data directory.

##### 3. Create the test network and pass the EVM params
   `--rewards-address` _is the address where you will receive your node earnings on._

```bash
cargo run --bin antctl -- local run --build --clean --rewards-address <YOUR_ETHEREUM_ADDRESS>
```

The EVM Network parameters are loaded from the CSV file in your data directory automatically when the `local` mode is enabled.

##### 4. Verify node status

```bash
cargo run --bin antctl -- status
```

The Antctl `run` command starts the node processes. The `status` command should show twenty-five
running nodes.

##### 5. Uploading and Downloading Data

To upload a file or a directory, you need to set the `SECRET_KEY` environment variable to your EVM secret key:

> When running a local network, you can use the `SECRET_KEY` printed by the `evm-testnet` command [step 2](#2-run-a-local-evm-node) as it has all the money.

```bash
SECRET_KEY=<YOUR_EVM_SECRET_KEY> cargo run --bin ant -- --local file upload <path>
```

The output will print out the address at which the content was uploaded.

Now to download the files again:

```bash
cargo run --bin ant -- --local file download <addr> <dest_path>
```

### Tear Down

When you're finished experimenting, tear down the network:

```bash
cargo run --bin antctl -- local kill
```

## Metrics Dashboard

Use the `open-metrics` feature flag on the node / client to start
an [OpenMetrics](https://github.com/OpenObservability/OpenMetrics/) exporter. The metrics are
served via a webserver started at a random port. Check the log file / stdout to find the webserver
URL, `Metrics server on http://127.0.0.1:xxxx/metrics`

The metrics can then be collected using a collector (for e.g. Prometheus) and the data can then be
imported into any visualization tool (for e.g., Grafana) to be further analyzed. Refer to
this [Guide](./metrics/README.md) to easily setup a dockerized Grafana dashboard to visualize the
metrics.

## Testing

The root of the repository provides a `test.sh` script that can run either unit tests, integration
tests, or a full suite of end to end tests. The integration tests do not require a running local
network, but the end to end tests do.

By default, `test.sh` will run the unit test suite.

See the sections below for further guidance.

### Test Levels

The test suite provides three distinct testing levels:

#### 1. Unit Tests (`./test.sh unit`)
Fast, isolated tests that don't require network connectivity:
- Tests individual functions and modules
- No network dependencies or EVM integration required
- Typically completes in under 30 seconds
- Tests core logic in isolation

```bash
./test.sh unit
```

Example output:
```
🧪 Unit Tests Summary
================================================
✅ ant-bootstrap: 8 tests passed
✅ ant-logging: 12 tests passed  
✅ autonomi (lib): 45 tests passed
✅ ant-networking: 23 tests passed
✅ ant-protocol: 15 tests passed
✅ node-launchpad: 7 tests passed

Total: 110 unit tests passed in 28.3s
```

#### 2. Integration Tests (`./test.sh integration`)
Network-based tests using a local test network:
- Requires local EVM testnet and Autonomi network
- Tests basic network connectivity and operations
- Moderate test duration (2-5 minutes)
- Validates network integration without comprehensive data operations

```bash
./test.sh integration
```

#### 3. Full Network Tests (`./test.sh full`)
Comprehensive end-to-end testing with complete logging infrastructure:
- Tests all data types: Chunks, Files, Registers, Vaults, GraphEntries, ScratchPads, Pointers
- Validates upload/download operations for every supported data structure
- Tests eventual consistency and network resilience
- Comprehensive logging with per-node log capture
- Automatic failure analysis and log interrogation
- Typically takes 10-15 minutes

```bash
./test.sh full
```

### Enhanced Logging and Analysis

The test runner includes sophisticated logging capabilities for debugging and failure analysis:

#### Command Line Options

```bash
# Enable debug logging and save all logs
./test.sh full --debug

# Save logs without debug verbosity  
./test.sh full --save-logs

# Enable automatic failure analysis
./test.sh full --analyze-failures

# Combine options
./test.sh full --debug --analyze-failures
```

#### Log Structure

When logging is enabled, tests create a timestamped directory structure:

```
/tmp/autonomi_test_run_2025-07-06_20-21-55/
├── logs/
│   ├── network/
│   │   ├── antctl.log           # Network startup logs
│   │   ├── evm-testnet.log      # EVM blockchain logs  
│   │   └── status-checks.log    # Network health monitoring
│   ├── tests/
│   │   ├── address.log          # Address/chunk tests
│   │   ├── files.log            # File upload/download tests
│   │   ├── vault.log            # Vault operations tests
│   │   ├── registers.log        # Register tests
│   │   ├── graph.log            # Graph entry tests
│   │   └── [other test logs]
│   ├── nodes/
│   │   ├── 12D3KooW.../logs/    # Individual node logs
│   │   └── [25 node directories]
│   └── summary/
│       ├── final-summary.txt    # Complete test summary
│       ├── test-runner.log      # Main execution log
│       └── failure-analysis.txt # Automatic failure analysis
```

#### Automatic Failure Analysis

When tests fail, the system automatically:

1. **Captures comprehensive logs** from all network components
2. **Analyzes failure patterns** in node logs and test output  
3. **Provides diagnostic summaries** with specific error locations
4. **Suggests remediation steps** based on common failure patterns

Example failure analysis:
```
🔍 FAILURE ANALYSIS
==================
Test: large.dat download failed
Location: /tmp/autonomi_test_run_2025-07-06_20-21-55/logs/tests/files.log:127

Error Pattern: "Connection timeout"
Node Logs: 3 nodes reported connectivity issues
Recommendation: Check network startup sequence and peer discovery

Detailed Logs:
- Network: /tmp/autonomi_test_run_2025-07-06_20-21-55/logs/network/
- Tests: /tmp/autonomi_test_run_2025-07-06_20-21-55/logs/tests/  
- Nodes: /tmp/autonomi_test_run_2025-07-06_20-21-55/logs/nodes/
```

### API vs CLI Test Distinction

The full test suite clearly separates:

#### API Integration Tests (Core Functionality)
Tests the core Rust client API and network integration:
- ✅ Address/chunk operations
- ✅ File upload/download
- ✅ Vault operations  
- ✅ Register management
- ✅ Graph entries
- ✅ Wallet functionality
- ✅ Payment processing

**When API tests pass → Core network functionality is healthy**

#### CLI Interface Tests (Optional/Diagnostic)  
Tests the command-line interface wrapper:
- Command parsing and validation
- File I/O operations
- User interface functionality

CLI test failures typically indicate interface issues rather than core network problems.

### Retrospective Analysis

Analyze previous test runs using saved logs:

```bash
# Analyze a specific test run
./test.sh analyze /tmp/autonomi_test_run_2025-07-06_20-21-55

# View summary of recent test runs
./test.sh analyze --recent
```

### Prerequisites for Testing

Before running network tests, ensure:

1. **Foundry/Anvil installed** for EVM testing:
   ```bash
   curl -L https://foundry.paradigm.xyz | bash
   foundryup
   ```

2. **Rust toolchain** up to date:
   ```bash
   rustup update
   ```

3. **Network ports available** (EVM testnet and node ports)

### Troubleshooting Common Issues

#### "Anvil not found"
- Install Foundry: `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- Ensure `~/.foundry/bin` is in your PATH

#### "Network startup timeout"  
- Check available memory (network requires ~2GB)
- Verify no conflicting processes on required ports
- Review network logs in test output directory

#### "Token insufficient errors"
- EVM testnet may not be ready - wait 10-15 seconds after startup
- Check EVM testnet logs for deployment issues

#### "Node connection failures"
- Network discovery can take 60-90 seconds for full connectivity
- Check individual node logs for connectivity issues
- Verify firewall isn't blocking local connections

### Running Tests in CI/CD

For automated testing environments:

```bash
# Run with comprehensive logging for CI analysis
./test.sh full --save-logs --analyze-failures

# Parse exit codes
if ./test.sh unit; then
    echo "Unit tests passed"
else  
    echo "Unit tests failed - check compilation issues"
    exit 1
fi
```

## Contributing

Feel free to clone and modify this project. Pull requests are welcome.

### Community

Join our community for support and discussions:
- **Discord**: https://discord.gg/autonomi
- **Forum**: https://forum.autonomi.community/
- **X (Twitter)**: https://x.com/WithAutonomi
- **Reddit**: https://www.reddit.com/r/autonomi/
- **LinkedIn**: https://uk.linkedin.com/company/withautonomi

For detailed contribution guidelines, see [CONTRIBUTING.md](CONTRIBUTING.md).

### Pull Request Process

1. Please direct all pull requests to the `main` branch.
2. Ensure that your commit messages clearly describe the changes you have made and use
   the [Conventional Commits](https://www.conventionalcommits.org/) specification.
3. All PRs must pass automated CI tests and peer review before being merged.
4. PRs should be <= 200 lines changed (lines added + lines deleted).
5. PRs should clearly reference an issue when applicable using [GitHub keywords](https://help.github.com/articles/closing-issues-using-keywords).

## License

This Autonomi Network repository is licensed under the General Public License (GPL), version
3 ([LICENSE](http://www.gnu.org/licenses/gpl-3.0.en.html)).
