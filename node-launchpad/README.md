# Autonomi Node Launchpad

A terminal user interface (TUI) for managing Autonomi network nodes. This tool provides an easy way to set up, monitor, and maintain nodes on the Autonomi decentralized network.

## Features

- **Simple node management**: Start, stop, and monitor multiple nodes from a single interface
- **Resource monitoring**: Track memory usage, bandwidth, and rewards earned by your nodes
- **Configuration options**: Customize connection modes, port settings, and storage locations
- **Wallet integration**: Link your wallet address to collect node rewards

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

The usage guides can be found here [docs.autonomi.com/node/guides/how-to-guides](https://docs.autonomi.com/node/guides/how-to-guides)

## Developer Notes

### Connecting to a Custom Network

The launchpad supports connecting to different Autonomi networks. Here is an example on how to spawn nodes using a
pre-built node binary and connect it to a network with a custom network ID.


| Option | Description |
|--------|-------------|
| `--network-id <ID>` | Specify the network ID to connect to. Default is 1 for mainnet |
| `--testnet` | Disable mainnet contacts (for test networks) |
| `--antnode-path <PATH>` | Path to the pre-built node binary |
| `--network-contacts-url <URL>` | Comma-separated list of URL containing the bootstrap cache. Can be ignored if `--peer` is used |
| `--peer <MULTIADDR>` | Comma-separated list of peer multiaddresses. Can be ignored if `--network-contacts-url` is used |


```bash
./node-launchpad --network-id 2 --testnet --antnode-path /path/to/antnode --peer /ip4/1.2.3.4/tcp/12000/p2p/12D3KooWAbCxMV2Zm3Pe4HcAokWDG9w8UMLpDiKpMxwLK3mixpkL
```
