# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Core Project Overview

The Autonomi Network is a fully autonomous data and communications network - critical infrastructure of the next web, assembled from everyday devices and owned by us all. The project consists of multiple interconnected Rust crates in a workspace structure that provides:

- **Lifetime Storage**: One-time payment for permanent data storage
- **Private by Design**: Multilayered encryption and self-encryption
- **Blockchainless Data**: No traditional consensus mechanism required
- **Decentralized Infrastructure**: Built from everyday devices forming a peer-to-peer network
- **Content-Addressable Storage**: Uses Kademlia DHT and libp2p
- **Client APIs**: Multiple interfaces including CLI, Python bindings, and Node.js bindings

For comprehensive documentation, visit: https://docs.autonomi.com/

## Essential Development Commands

### Building and Testing
```bash
# Build all workspace members
cargo build --release

# Run unit tests (specific packages as they require different setups)
cargo test --release --package autonomi --lib
cargo test --release --package ant-bootstrap
cargo test --release --package ant-node --lib
cargo test --release --package node-launchpad
cargo test --release --package ant-networking --features="open-metrics"
cargo test --release --package ant-protocol
cargo test --release --package ant-logging

# Run E2E tests (requires local network setup)
cargo test --package autonomi --tests

# Check code formatting
cargo fmt --all -- --check

# Run clippy linter
cargo clippy --all-targets --all-features -- -Dwarnings

# Check documentation
RUSTDOCFLAGS="--deny=warnings" cargo doc --no-deps --workspace
```

### Local Network Development
```bash
# Start local EVM testnet (required for payments)
cargo run --bin evm-testnet

# Create local test network with 25 nodes
cargo run --bin antctl -- local run --build --clean --rewards-address <YOUR_ETHEREUM_ADDRESS>

# Check node status
cargo run --bin antctl -- status

# Upload files (requires SECRET_KEY environment variable)
SECRET_KEY=<YOUR_EVM_SECRET_KEY> cargo run --bin ant -- --local file upload <path>

# Download files
cargo run --bin ant -- --local file download <addr> <dest_path>

# Tear down network
cargo run --bin antctl -- local kill
```

### Binary Builds
```bash
# Build release binaries using justfile
just build-release-artifacts <architecture>

# Supported architectures: x86_64-pc-windows-msvc, x86_64-apple-darwin, 
# aarch64-apple-darwin, x86_64-unknown-linux-musl, arm-unknown-linux-musleabi,
# armv7-unknown-linux-musleabihf, aarch64-unknown-linux-musl
```

## Workspace Architecture

### Core Node Components
- **ant-node**: The backbone node implementation that stores data and validates transactions
- **ant-networking**: P2P networking layer built on libp2p with Kademlia DHT
- **ant-protocol**: Network protocol definitions and message types
- **ant-node-manager**: Tools for managing local networks and node processes

### Client Components  
- **autonomi**: Main client API library with high-level data operations
- **ant-cli**: Command-line interface for end users
- **nodejs**: Node.js bindings for the client API
- **autonomi/python**: Python bindings for the client API

### Supporting Components
- **ant-bootstrap**: Network bootstrap peer discovery and caching
- **ant-logging**: Centralized logging infrastructure using tracing
- **ant-metrics**: OpenMetrics exporter for monitoring and observability
- **evmlib**: EVM blockchain integration for payments
- **ant-evm**: Payment processing and blockchain interaction

### Development Tools
- **node-launchpad**: TUI application for managing nodes
- **test-utils**: Shared testing utilities across the workspace

## Key Development Patterns

### Testing Strategy
- Unit tests are run per package due to different requirements and network dependencies
- E2E tests require a running local network (set up via antctl local run)
- Integration tests use the ant-local-testnet-action for GitHub Actions
- Churn tests simulate network instability and node restarts

### Payment System Integration
All data operations require EVM payments. When developing:
- Use the local EVM testnet for development (`cargo run --bin evm-testnet`)
- Set SECRET_KEY environment variable for client operations
- Payments are handled through the evmlib crate

### Network Architecture
- Nodes form a Kademlia DHT for routing and data location
- Data is stored as encrypted records inside each node
- There are multiple data types that are supported natively
- QUIC is the primary transport protocol
- Initial network bootstrapping happens by dialing the bootstrap peers from ant-bootstrap
- Periodic network discovery happens on the nodes to keeps the peers in the RT fresh

### Code Quality Standards
- All code must pass clippy with no warnings (`-Dwarnings`)
- Formatting enforced via rustfmt
- Documentation required (checked with `--deny=warnings`)
- Unwrap is allowed in tests (see .clippy.toml)

## Branch and Release Process
- Main development happens on the `main` branch
- PRs should target `main`
- Use Conventional Commits specification for commit messages
- Release process is automated via CI/CD
- Building for production should use the `stable` branch