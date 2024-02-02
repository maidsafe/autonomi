 # The Safe Network

[SafenetForum.org](https://safenetforum.org/)

Own your data. Share your disk space. Get paid for doing so.<br>
The Data on the Safe Network is Decentralised, Autonomous, and  built atop of Kademlia and Libp2p.<br>

## Table of Contents

- [For Users](#for-Users)
- [For Developers](#for-developers)
- [For the Technical](#for-the-technical)
- [Using a Local Network](#Using-a-local-network)
- [Metrics Dashboard](#metrics-dashboard)

### For Users

- [CLI](https://github.com/maidsafe/safe_network/blob/main/sn_cli/README.md) The Command Line Interface, allowing users to interact with the network from their terminal.
- [Node](https://github.com/maidsafe//safe_network/blob/main/sn_node/README.md) The backbone of the safe network. Nodes can be run on commodity hardware and provide storage space and validation of transactions to the network.

### For Developers

- [Client](https://github.com/maidsafe/safe_network/blob/main/sn_client/README.md) The client APIs allowing use of the SafeNetwork to users and developers.
- [Registers](https://github.com/maidsafe/safe_network/blob/main/sn_registers/README.md) The CRDT registers structures available on the network.
- [Node Manager](https://github.com/maidsafe/safe_network/blob/main/sn_node_manager/README.md) Use to create a local network for development and testing.
- [Faucet](https://github.com/maidsafe/safe_network/blob/main/sn_faucet/README.md) The local faucet server, used to claim genesis and request tokens from the network.
- [Node RPC](https://github.com/maidsafe/safe_network/blob/main/sn_node_rpc_client/README.md) The RPC server used by the nodes to expose API calls to the outside world.

#### Transport Protocols and Architectures

The Safe Network uses `quic` as the default transport protocol.

The `websockets` feature is available for the `sn_networking` crate, and above, and will allow for tcp over websockets.

If building for `wasm32` then `websockets` are enabled by default as this is the only method avilable to communicate with a network as things stand. (And that network must have `websockets` enabled.)

##### Building for wasm32

- Install [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)
- `cd sn_client && wasm-pack build`

You can then pull this package into a web app eg, to use it.

eg `await safe.get_data("/ip4/127.0.0.1/tcp/59324/ws/p2p/12D3KooWG6kyBwLVHj5hYK2SqGkP4GqrCz5gfwsvPBYic4c4TeUz","9d7e115061066126482a229822e6d68737bd67d826c269762c0f64ce87af6b4c")`

#### Browser usage

Browser usage is highly experimental, but the wasm32 target for `sn_client` _should_ work here. YMMV until stabilised.

### For the Technical

- [Logging](https://github.com/maidsafe/safe_network/blob/main/sn_logging/README.md) The generalised logging crate used by the safe network (backed by the tracing crate).
- [Metrics](https://github.com/maidsafe/safe_network/blob/main/metrics/README.md) The metrics crate used by the safe network.
- [Networking](https://github.com/maidsafe/safe_network/blob/main/sn_networking/README.md) The networking layer, built atop libp2p which allows nodes and clients to communicate.
- [Protocol](https://github.com/maidsafe/safe_network/blob/main/sn_protocol/README.md) The protocol used by the safe network.
- [Transfers](https://github.com/maidsafe/safe_network/blob/main/sn_transfers/README.md) The transfers crate, used to send and receive tokens on the network.
- [Peers Acquisition](https://github.com/maidsafe/safe_network/blob/main/sn_peers_acquisition/README.md) The peers peers acqisition crate, or: how the network layer discovers bootstrap peers.
- [Build Info](https://github.com/maidsafe/safe_network/blob/main/sn_build_info/README.md) Small helper used to get the build/commit versioning info for debug purposes.

## Using a Local Network

We can explore the network's features by using multiple node processes to form a local network.

The latest version of [Rust](https://www.rust-lang.org/learn/get-started) should be installed. If you already have an installation, use `rustup update` to get the latest version.

Run all the commands from the root of this repository.

### Run the Network

Follow these steps to create a local network:
1. Create the test network: <br>
```bash
cargo run --bin safenode-manager --features local-discovery -- run --build
```
2. Verify node status: <br>
```bash
cargo run --bin safenode-manager --features local-discovery -- status
```
3. Build a tokenized wallet: <br>
```bash
cargo run --bin safe --features local-discovery -- wallet get-faucet 127.0.0.1:8000
```

The node manager's `run` command starts the node processes and a faucet process, the latter of which will dispense tokens for use with the network. The `status` command should show twenty-five running nodes. The `wallet` command retrieves some tokens, which enables file uploads.

### Files

The file storage capability can be demonstrated by uploading files to the local network, then retrieving them.

Upload a file or a directory:
```bash
cargo run --bin safe --features local-discovery -- files upload <path>
```

The output will show that the upload costs some tokens.

Now download the files again:
```bash
cargo run --bin safe --features local-discovery -- files download
```

### Token Transfers

Use your local wallet to demonstrate sending tokens and receiving transfers.

First, get your wallet address:
```
cargo run --bin safe -- wallet address
```

Now send some tokens to that address: 
```
cargo run --bin safe --features local-discovery -- wallet send 2 [address]
```

This will output a transfer as a hex string, which should be sent to the recipient out-of-band.

For demonstration purposes, copy the transfer string and use it to receive the transfer in your own wallet:
```
cargo run --bin safe --features local-discovery -- wallet receive [transfer]
```

#### Out of band transaction signing

Steps on the online device/computer:
1. Create a watch-only wallet using the hex-encoded public key:
  `cargo run --release --bin safe -- wowallet create <hex-encoded public key>`

2. Deposit a cash-note, owned by the public key used above when creating, into the watch-only wallet:
  `cargo run --release --bin safe -- wowallet deposit <hex-encoded public key> --cash-note <hex-encoded cash-note>`

3. Build an unsigned transaction:
  `cargo run --release --bin safe -- wowallet transaction <hex-encoded public key> <amount> <recipient's hex-encoded public key>`

4. Copy the built unsigned Tx generated by the above command, and send it out-of-band to the desired device where the hot-wallet can be loaded.

Steps on the offline device/computer:

5. If you still don't have a hot-wallet created, which owns the cash-notes used to build the unsigned transaction, create it with the corresponding secret key: 
  `cargo run --release --bin safe -- wallet create <hex-encoded secret key>`

6. Use the hot-wallet to sign the built transaction:
  `cargo run --release --bin safe -- wallet sign <unsigned transaction>`

7. Copy the signed Tx generated by the above command, and send it out-of-band back to the online device.

Steps on the online device/computer:

8. Broadcast the signed transaction to the network using the watch-only wallet:
  `cargo run --release --bin safe -- wowallet broadcast <signed transaction>`

9. Deposit the change cash-note to the watch-only wallet:
  `cargo run --release --bin safe -- wowallet deposit <hex-encoded public key> <change cash-note>`

10. Send/share the output cash-note generated by the above command at step #8 to/with the recipient.


### Auditing

We can verify a spend, optionally going back to the genesis transaction:
```
cargo run --bin safe --features local-discovery -- wallet verify [--genesis] [spend address]
```

All spends from genesis can be audited:
```
cargo run --bin safe --features local-discovery -- wallet audit
```

### Registers

Registers are one of the network's data types. The workspace here has an example app demonstrating
their use:
```
cargo run --example registers --features=local-discovery -- --user alice --reg-nickname myregister
```

### RPC

The node manager launches each node process with a remote procedure call (RPC) service. The workspace has a client binary that can be used to run commands against these services.

Run the `status` command with the `--details` flag to get the RPC port for each node:
```
$ cargo run --bin safenode-manager -- status --details
...
===================================
safenode-local25 - RUNNING
===================================
Version: 0.103.21
Peer ID: 12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
Port: 38835
RPC Port: 34416
Multiaddr: /ip4/127.0.0.1/udp/38835/quic-v1/p2p/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
PID: 62369
Data path: /home/chris/.local/share/safe/node/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
Log path: /home/chris/.local/share/safe/node/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8/logs
Bin path: target/release/safenode
Connected peers: 24
```

Now you can run RPC commands against any node.

The `info` command will retrieve basic information about the node:
```
$ cargo run --bin safenode_rpc_client -- 127.0.0.1:34416 info
Node info:
==========
RPC endpoint: https://127.0.0.1:34416
Peer Id: 12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8
Logs dir: /home/chris/.local/share/safe/node/12D3KooWJ4Yp8CjrbuUyeLDsAgMfCb3GAYMoBvJCRp1axjHr9cf8/logs
PID: 62369
Binary version: 0.103.21
Time since last restart: 1614s
```

The `netinfo` command will return connected peers and listeners:
```
$ cargo run --bin safenode_rpc_client -- 127.0.0.1:34416 netinfo
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
$ cargo run --bin safenode_rpc_client -- 127.0.0.1:34416 restart 5000
Node successfully received the request to restart in 5s

$ cargo run --bin safenode_rpc_client -- 127.0.0.1:34416 stop 6000
Node successfully received the request to stop in 6s

$ cargo run --bin safenode_rpc_client -- 127.0.0.1:34416 update 7000
Node successfully received the request to try to update in 7s
```

NOTE: it is preferable to use the node manager to control the node rather than RPC commands.

Listening to royalty payment events:
```
$ cargo run --bin safenode_rpc_client -- 127.0.0.1:34416 transfers
Listening to transfers notifications... (press Ctrl+C to exit)

New transfer notification received for PublicKey(0c54..5952), containing 1 cash note/s.
CashNote received with UniquePubkey(PublicKey(19ee..1580)), value: 0.000000001

New transfer notification received for PublicKey(0c54..5952), containing 1 cash note/s.
CashNote received with UniquePubkey(PublicKey(19ee..1580)), value: 0.000000001
```

The `transfers` command can provide a path for royalty payment cash notes:
```
$ cargo run --release --bin=safenode_rpc_client -- 127.0.0.1:34416 transfers ./royalties-cash-notes
Listening to transfers notifications... (press Ctrl+C to exit)
Writing cash notes to: ./royalties-cash-notes
```

Each received cash note is written to a file in the directory above, under another directory corresponding to the public address of the recipient.

### Tear Down

When you're finished experimenting, tear down the network:
```
cargo run --bin safenode-manager -- kill
```

## Metrics Dashboard

Use the `open-metrics` feature flag on the node / client to start an [OpenMetrics](https://github.com/OpenObservability/OpenMetrics/) exporter. The metrics are served via a webserver started at a random port. Check the log file / stdout to find the webserver URL, `Metrics server on http://127.0.0.1:xxxx/metrics`

The metrics can then be collected using a collector (for e.g. Prometheus) and the data can then be imported into any visualization tool (for e.g., Grafana) to be further analyzed. Refer to this [Guide](./metrics/README.md) to easily setup a dockerized Grafana dashboard to visualize the metrics.

## Contributing

Feel free to clone and modify this project. Pull requests are welcome.<br>You can also visit **[The MaidSafe Forum](https://safenetforum.org/)** for discussion or if you would like to join our online community.

### Conventional Commits

We follow the [Conventional Commits](https://www.conventionalcommits.org/) specification for all commits. Please make sure your commit messages adhere to this standard.

## License

This Safe Network repository is licensed under the General Public License (GPL), version 3 ([LICENSE](http://www.gnu.org/licenses/gpl-3.0.en.html)).
