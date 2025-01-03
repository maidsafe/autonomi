# The Autonomi Network (previously Safe Network)

[Autonomi.com](https://autonomi.com/)

Own your data. Share your disk space. Get paid for doing so.<br>
The Data on the Autonomi Network is Decentralised, Autonomous, and built atop of Kademlia and
Libp2p.<br>

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
- [Registers](https://github.com/maidsafe/autonomi/blob/main/ant-registers/README.md) The
  registers crate, used for the Register CRDT data type on the network.
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
cargo run --bin antctl --features local -- local run --build --clean --rewards-address <YOUR_ETHEREUM_ADDRESS>
```

The EVM Network parameters are loaded from the CSV file in your data directory automatically when the `local` feature flag is enabled (`--features=local`).

##### 4. Verify node status

```bash
cargo run --bin antctl --features local -- status
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

### Registers

Registers are one of the network's data types. The workspace here has an example app demonstrating
their use by two users to exchange text messages in a crude chat application.

In the first terminal, using the registers example, Alice creates a register:

```
cargo run --example registers -- --local --user alice --reg-nickname myregister
```

Alice can now write a message to the register and see anything written by anyone else. For example
she might enter the text "Hello, who's there?" which is written to the register and then shown as
the "Latest value", in her terminal:

```
Register address: "50f4c9d55aa1f4fc19149a86e023cd189e509519788b4ad8625a1ce62932d1938cf4242e029cada768e7af0123a98c25973804d84ad397ca65cb89d6580d04ff07e5b196ea86f882b925be6ade06fc8d"
Register owned by: PublicKey(0cf4..08a5)
Register permissions: Permissions { anyone_can_write: true, writers: {PublicKey(0cf4..08a5)} }

Current total number of items in Register: 0
Latest value (more than one if concurrent writes were made):
--------------
--------------

Enter a blank line to receive updates, or some text to be written.
Hello, who's there?
Writing msg (offline) to Register: 'Hello, who's there?'
Syncing with SAFE in 2s...
synced!

Current total number of items in Register: 1
Latest value (more than one if concurrent writes were made):
--------------
[Alice]: Hello, who's there?
--------------

Enter a blank line to receive updates, or some text to be written.

```

For anyone else to write to the same register they need to know its xor address, so to communicate
with her friend Bob, Alice needs to find a way to send it to Bob. In her terminal, this is the
value starting "50f4..." in the output above. This value will be different each time you run the
example to create a register.

Having received the xor address, in another terminal Bob can access the same register to see the
message Alice has written, and he can write back by running this command with the address received
from Alice. (Note that the command should all be on one line):

```
cargo run --example registers -- --local --user bob --reg-address 50f4c9d55aa1f4fc19149a86e023cd189e509519788b4ad8625a1ce62932d1938cf4242e029cada768e7af0123a98c25973804d84ad397ca65cb89d6580d04ff07e5b196ea86f882b925be6ade06fc8d
```

After retrieving the register and displaying the message from Alice, Bob can reply and at any time,
Alice or Bob can send another message and see any new messages which have been written, or enter a
blank line to poll for updates.

Here's Bob writing from his terminal:

```
Latest value (more than one if concurrent writes were made):
--------------
[Alice]: Hello, who's there?
--------------

Enter a blank line to receive updates, or some text to be written.
hi Alice, this is Bob!
```

Alice will see Bob's message when she either enters a blank line or writes another message herself.

### Inspect a Register

A second example, `register_inspect` allows you to view its structure and content. To use this with
the above example you again provide the address of the register. For example:

```
cargo run --example register_inspect -- --local --reg-address 50f4c9d55aa1f4fc19149a86e023cd189e509519788b4ad8625a1ce62932d1938cf4242e029cada768e7af0123a98c25973804d84ad397ca65cb89d6580d04ff07e5b196ea86f882b925be6ade06fc8d
```

After printing a summary of the register, this example will display
the structure of the register each time you press Enter, including the following:

```
Enter a blank line to print the latest register structure (or 'Q' <Enter> to quit)

Syncing with SAFE...
synced!
======================
Root (Latest) Node(s):
[ 0] Node("4eadd9"..) Entry("[alice]: this is alice 3")
[ 3] Node("f05112"..) Entry("[bob]: this is bob 3")
======================
Register Structure:
(In general, earlier nodes are more indented)
[ 0] Node("4eadd9"..) Entry("[alice]: this is alice 3")
  [ 1] Node("f5afb2"..) Entry("[alice]: this is alice 2")
    [ 2] Node("7693eb"..) Entry("[alice]: hello this is alice")
[ 3] Node("f05112"..) Entry("[bob]: this is bob 3")
  [ 4] Node("8c3cce"..) Entry("[bob]: this is bob 2")
    [ 5] Node("c7f9fc"..) Entry("[bob]: this is bob 1")
    [ 1] Node("f5afb2"..) Entry("[alice]: this is alice 2")
      [ 2] Node("7693eb"..) Entry("[alice]: hello this is alice")
======================
```

Each increase in indentation shows the children of the node above.
The numbers in square brackets are just to make it easier to see
where a node occurs more than once.

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

Feel free to clone and modify this project. Pull requests are welcome.<br>You can also
visit \* \*[The MaidSafe Forum](https://safenetforum.org/)\*\* for discussion or if you would like to join our
online community.

### Pull Request Process

1. Please direct all pull requests to the `alpha` branch instead of the `main` branch.
1. Ensure that your commit messages clearly describe the changes you have made and use
   the [Conventional Commits](https://www.conventionalcommits.org/) specification.

## License

This Safe Network repository is licensed under the General Public License (GPL), version
3 ([LICENSE](http://www.gnu.org/licenses/gpl-3.0.en.html)).
