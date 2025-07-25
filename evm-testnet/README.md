## EVM Testnet

Tool to run a local Ethereum node that automatically deploys all Autonomi smart contracts.

### Requirements

1. Install Foundry to get access to Anvil nodes: https://book.getfoundry.sh/getting-started/installation

### Usage

```bash
cargo run --bin evm-testnet -- --genesis-wallet <ETHEREUM_ADDRESS>
```

Example output:

```
*************************
* Ethereum node started *
*************************
RPC URL: http://localhost:61611/
Payment token address: 0x5FbDB2315678afecb367f032d93F642f64180aa3
Chunk payments address: 0x8464135c8F25Da09e49BC8782676a84730C318bC
Deployer wallet private key: 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
Genesis wallet balance: (tokens: 20000000000000000000000000, gas: 9998998011366954730202)
```
