import test from 'ava'
import { Network, NetworkSpawner, NodeSpawner } from '../index.js'
import { setTimeout } from 'node:timers/promises';

test('construct NetworkSpawner', async (t) => {
    const networkSize = 20;

    const spawner = new NetworkSpawner({ local: true, size: networkSize });
    const runningNetwork = await spawner.spawn();

    const runningNodes = await runningNetwork.runningNodes();
    t.is(runningNodes.length, networkSize);

    await setTimeout(10 * 1000);

    for (const node of runningNodes) {
        const peersInRoutingTable = (await node.getSwarmLocalState()).peersInRoutingTable;

        t.true(peersInRoutingTable >= networkSize - 2 && peersInRoutingTable < networkSize);
    }

    await runningNetwork.shutdown();
});

test('construct NodeSpawner', async (t) => {
    const spawner = new NodeSpawner({ local: true }, Network.fromString('evm-arbitrum-one'));
    const runningNode = await spawner.spawn();
    runningNode.shutdown();

    t.true(typeof runningNode.peerId() === 'string' && runningNode.peerId().length > 0);
});
