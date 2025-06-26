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
- [Node RPC](https://github.com/maidsafe/autonomi/blob/main/ant-node-rpc-client/README.md) The
  RPC server used by the nodes to expose API calls to the outside world.

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

### RPC

The node manager launches each node process with a remote procedure call (RPC) service. The
workspace has a client binary that can be used to run commands against these services.

Run the `status` command with the `--details` flag to get the RPC port for each node:

```
$ cargo run --bin antctl -- status --details
...
===================================
antctl-local25 - RUNNING
===================================
Version: 0.103.21
Peer ID: 12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
Port: 38835
RPC Port: 34416
Multiaddr: /ip4/127.0.0.1/udp/38835/quic-v1/p2p/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
PID: 62369
Data path: /home/<<user_directory>>/.local/share/autonomi/node/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
Log path: /home/<<user_directory>>/.local/share/autonomi/node/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8/logs
Bin path: target/release/antnode
Connected peers: 24
```

Now you can run RPC commands against any node.

The `info` command will retrieve basic information about the node:

```
$ cargo run --bin antnode_rpc_client -- 127.0.0.1:34416 info
Node info:
==========
RPC endpoint: https://127.0.0.1:34416
Peer Id: 12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
Logs dir: /home/<<user_directory>>/.local/share/autonomi/node/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8/logs
PID: 62369
Binary version: 0.103.21
Time since last restart: 1614s
```

The `netinfo` command will return connected peers and listeners:

```
$ cargo run --bin antnode_rpc_client -- 127.0.0.1:34416 netinfo
Node's connections to the Network:

Connected peers:
Peer: 12D3KooWJkD2pB2WdczBJWt4ZSAWfFFMa8FHe6w9sKvH2mZ6RKdm
Peer: 12D3KooWRNCqFYX8dJKcSTAgxcy5CLMcEoM87ZSzeF43kCVCCFnc
Peer: 12D3KooWLDUFPR2jCZ88pyYCNMZNa4PruweMsZDJXUvVeg1sSMtN
Peer: 12D3KooWC8GR5NQeJwTsvn9SKChRZqJU8XS8ZzKPwwgBi63FHdUQ
Peer: 12D3KooWJGERJnGd5N814V295zq1CioxUUWKgNZy4zJmBLodAPEj
Peer: 12D3KooWJ9KHPwwiRpgxwhwsjCiHecvkr2w3JsUQ1MF8q9gzWV6U
Peer: 12D3KooWSBafke1pzz3KUXbH875GYcMLVqVht5aaXNSRtbie6G9g
Peer: 12D3KooWJtKc4C7SRkei3VURDpnsegLUuQuyKxzRpCtsJGhakYfX
Peer: 12D3KooWKg8HsTQ2XmBVCeGxk7jHTxuyv4wWCWE2pLPkrhFHkwXQ
Peer: 12D3KooWQshef5sJy4rEhrtq2cHGagdNLCvcvMn9VXwMiLnqjPFA
Peer: 12D3KooWLfXHapVy4VV1DxWndCt3PmqkSRjFAigsSAaEnKzrtukD

Node's listeners:
Listener: /ip4/127.0.0.1/udp/38835/quic-v1
Listener: /ip4/192.168.1.86/udp/38835/quic-v1
Listener: /ip4/172.17.0.1/udp/38835/quic-v1
Listener: /ip4/172.18.0.1/udp/38835/quic-v1
Listener: /ip4/172.20.0.1/udp/38835/quic-v1
```

Node control commands:

```
$ cargo run --bin antnode_rpc_client -- 127.0.0.1:34416 restart 5000
Node successfully received the request to restart in 5s

$ cargo run --bin antnode_rpc_client -- 127.0.0.1:34416 stop 6000
Node successfully received the request to stop in 6s

$ cargo run --bin antnode_rpc_client -- 127.0.0.1:34416 update 7000
Node successfully received the request to try to update in 7s
```

NOTE: it is preferable to use the node manager to control the node rather than RPC commands.

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
