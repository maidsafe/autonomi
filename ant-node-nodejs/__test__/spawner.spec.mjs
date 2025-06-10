import test from 'ava'
import { NetworkSpawner } from '../index.js'
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
