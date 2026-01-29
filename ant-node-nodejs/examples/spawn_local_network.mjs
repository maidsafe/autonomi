#!/usr/bin/env node
// Copyright 2026 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

/**
 * # Example: Spawn a local Autonomi network and get a cost quote
 *
 * This example shows how to:
 * 1. Start an EVM testnet (Anvil)
 * 2. Spawn a local Autonomi network (25 nodes)
 * 3. Connect an Autonomi client to the network
 * 4. Get a cost quote for storing 1MB of data
 *
 * ## Prerequisites
 *
 * 1. Build the Node.js bindings:
 *    ```bash
 *    cd ant-node-nodejs && npm install && npm run build
 *    cd ../autonomi-nodejs && npm install && npm run build
 *    ```
 *
 * 2. Install Foundry (provides Anvil for local EVM):
 *    - macOS/Linux: curl -L https://foundry.paradigm.xyz | bash && foundryup
 *    - Windows: irm https://foundry.paradigm.xyz | iex && foundryup
 *
 * ## Run
 * ```bash
 * node ant-node-nodejs/examples/spawn_local_network.mjs
 * ```
 */

import { NetworkSpawner, Testnet } from '../index.js';
import { Client } from '../../autonomi-nodejs/index.js';
import { setTimeout } from 'node:timers/promises';

const NETWORK_SIZE = 25;
const DISCOVERY_WAIT_SECS = 15;

async function main() {
    console.log('=== Spawn Local Network Example ===\n');

    // Step 1: Start EVM testnet (same as Rust example)
    console.log('Starting EVM testnet...');
    const testnet = await Testnet.new();
    const evmNetwork = testnet.toNetwork();
    console.log(`EVM testnet ready at ${testnet.rpcUrl()}`);

    // Step 2: Spawn local Autonomi network
    console.log(`\nSpawning ${NETWORK_SIZE} nodes...`);

    const bootstrapConfig = {
        local: true,
        disableCacheReading: true,
        disableCacheWriting: true,
        disableEnvPeers: true
    };

    // WARNING: Using no rewards address means zero address (0x0...0).
    // Any rewards would be lost! This is fine ONLY for local testing.
    const spawner = new NetworkSpawner({
        noUpnp: true,
        size: NETWORK_SIZE,
        bootstrapConfig
    }, evmNetwork);

    const runningNetwork = await spawner.spawn();
    const runningNodes = await runningNetwork.runningNodes();

    console.log(`Network spawned with ${runningNodes.length} nodes`);

    // Show spawned nodes
    for (const [i, node] of runningNodes.entries()) {
        const peerId = node.peerId();
        console.log(`Node ${i + 1}: ${peerId.substring(0, 20)}...`);
    }

    console.log(`\nWaiting ${DISCOVERY_WAIT_SECS} seconds for peer discovery...`);
    await setTimeout(DISCOVERY_WAIT_SECS * 1000);

    // Step 3: Connect client to the local network
    console.log('Connecting client to network...');
    const bootstrapPeer = await runningNetwork.bootstrapPeer();
    console.log(`Bootstrap peer: ${bootstrapPeer.substring(0, 50)}...`);

    const client = await Client.initWithPeers([bootstrapPeer]);
    console.log('Client connected!');

    // Step 4: Get a cost quote
    console.log('\nGetting cost quote for 1MB of data...');
    const data = Buffer.alloc(1024 * 1024); // 1MB of zeros
    const cost = await client.dataCost(data);
    console.log(`Cost to store 1MB: ${cost}`);

    // Cleanup
    console.log('\nShutting down...');
    await runningNetwork.shutdown();
    console.log('Done!');
}

main().catch(err => {
    console.error('Error:', err);
    process.exit(1);
});
